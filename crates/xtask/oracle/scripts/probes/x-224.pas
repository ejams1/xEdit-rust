{ parity probe: x-224 }
unit P_x_224;

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
  v := Assigned(Unassigned); AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
