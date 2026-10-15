{ parity probe: x-338 }
unit P_x_338;

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
  v := TObject.Create; AddMessage(v.ClassName);
  AddMessage('after');
end;
end.
