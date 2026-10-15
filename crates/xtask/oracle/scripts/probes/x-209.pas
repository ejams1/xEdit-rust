{ parity probe: x-209 }
unit P_x_209;

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
  v := sl = sl; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
