{ parity probe: x-326 }
unit P_x_326;

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
  v := sl is TList; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
