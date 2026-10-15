{ parity probe: x-325 }
unit P_x_325;

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
  v := sl is TStringList; AddMessage(VarToStr(v));
  AddMessage('after');
end;
end.
