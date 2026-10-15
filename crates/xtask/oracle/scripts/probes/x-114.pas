{ parity probe: x-114 }
unit P_x_114;

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
  s := 'x'; s := s + 1.5; AddMessage(s);
  AddMessage('after');
end;
end.
