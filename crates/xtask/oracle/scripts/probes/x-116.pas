{ parity probe: x-116 }
unit P_x_116;

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
  s := 'x'; s := s + nil; AddMessage(s);
  AddMessage('after');
end;
end.
