{ parity probe: x-228 }
unit P_x_228;

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
  v := FileByIndex(0); v := v = nil; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
