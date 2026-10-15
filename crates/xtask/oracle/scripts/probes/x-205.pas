{ parity probe: x-205 }
unit P_x_205;

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
  v := nil = nil; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
