{ parity probe: x-226 }
unit P_x_226;

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
  v := Assigned(ElementByIndex(FileByIndex(0), 99)); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
