{ parity probe: x-117 }
unit P_x_117;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  s := 'x'; s := s + Null; AddMessage(s);
  AddMessage('after');
end;
end.
