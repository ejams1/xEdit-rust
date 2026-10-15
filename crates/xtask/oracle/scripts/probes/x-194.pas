{ parity probe: x-194 }
unit P_x_194;

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
  v := -'5'; AddMessage(VarToStr(v) + ' ' + IntToStr(VarType(v)));
  AddMessage('after');
end;
end.
