{ parity probe: x-208 }
unit P_x_208;

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
  v := sl <> nil; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
