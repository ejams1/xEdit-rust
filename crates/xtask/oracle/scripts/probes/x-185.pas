{ parity probe: x-185 }
unit P_x_185;

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
  s := 1 = 1; AddMessage(s);
  AddMessage('after');
end;
end.
