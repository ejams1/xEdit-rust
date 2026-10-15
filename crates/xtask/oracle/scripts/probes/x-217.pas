{ parity probe: x-217 }
unit P_x_217;

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
  v := Assigned(sl); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
